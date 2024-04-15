use {
    self::{
        get_notifications::{GetNotifications, GetNotificationsParams},
        mark_notifications_as_read::{MarkNotificationsAsRead, MarkNotificationsAsReadParams},
        notification_link::{NotificationLink, NotificationLinkParams},
        relay_request::{RelayRequest, RelayResponseParams},
        subscriber_notification::{SubscriberNotification, SubscriberNotificationParams},
        subscriber_update::{SubscriberUpdate, SubscriberUpdateParams},
    },
    crate::config::Configuration,
    aws_sdk_s3::Client as S3Client,
    parquet::record::RecordWriter,
    std::{net::IpAddr, sync::Arc, time::Duration},
    tracing::{error, info},
    wc::{
        analytics::{
            self, AnalyticsExt, ArcCollector, AwsConfig, AwsExporter, BatchCollector,
            BatchObserver, CollectionObserver, Collector, CollectorConfig, ExportObserver,
            ParquetBatchFactory,
        },
        geoip::{self, MaxMindResolver, Resolver},
        metrics::otel,
    },
};

pub mod get_notifications;
pub mod mark_notifications_as_read;
pub mod notification_link;
pub mod relay_request;
pub mod subscriber_notification;
pub mod subscriber_update;

const ANALYTICS_EXPORT_TIMEOUT: Duration = Duration::from_secs(30);
const DATA_QUEUE_CAPACITY: usize = 8192;

#[derive(Clone, Copy)]
enum DataKind {
    SubscriberNotifications,
    SubscriberUpdates,
    GetNotifications,
    MarkNotificationsAsRead,
    NotificationLinks,
    RelayRequests,
}

impl DataKind {
    #[inline]
    fn as_str(&self) -> &'static str {
        match self {
            Self::SubscriberNotifications => "subscriber_notifications",
            Self::SubscriberUpdates => "subscriber_updates",
            Self::GetNotifications => "get_notifications",
            Self::MarkNotificationsAsRead => "mark_notifications_as_read",
            Self::NotificationLinks => "notification_links",
            Self::RelayRequests => "relay_requests",
        }
    }

    #[inline]
    fn as_kv(&self) -> otel::KeyValue {
        otel::KeyValue::new("data_kind", self.as_str())
    }
}

fn success_kv(success: bool) -> otel::KeyValue {
    otel::KeyValue::new("success", success)
}

#[derive(Clone, Copy)]
struct Observer(DataKind);

impl<T, E> BatchObserver<T, E> for Observer
where
    E: std::error::Error,
{
    fn observe_batch_serialization(&self, elapsed: Duration, res: &Result<Vec<u8>, E>) {
        let size = res.as_deref().map(|data| data.len()).unwrap_or(0);
        let elapsed = elapsed.as_millis() as u64;

        wc::metrics::counter!(
            "analytics_batches_finished",
            1,
            &[self.0.as_kv(), success_kv(res.is_ok())]
        );

        if let Err(err) = res {
            tracing::warn!(
                ?err,
                data_kind = self.0.as_str(),
                "failed to serialize analytics batch"
            );
        } else {
            tracing::info!(
                size,
                elapsed,
                data_kind = self.0.as_str(),
                "analytics data batch serialized"
            );
        }
    }
}

impl<T, E> CollectionObserver<T, E> for Observer
where
    E: std::error::Error,
{
    fn observe_collection(&self, res: &Result<(), E>) {
        wc::metrics::counter!(
            "analytics_records_collected",
            1,
            &[self.0.as_kv(), success_kv(res.is_ok())]
        );

        if let Err(err) = res {
            tracing::warn!(
                ?err,
                data_kind = self.0.as_str(),
                "failed to collect analytics data"
            );
        }
    }
}

impl<E> ExportObserver<E> for Observer
where
    E: std::error::Error,
{
    fn observe_export(&self, elapsed: Duration, res: &Result<(), E>) {
        wc::metrics::counter!(
            "analytics_batches_exported",
            1,
            &[self.0.as_kv(), success_kv(res.is_ok())]
        );

        let elapsed = elapsed.as_millis() as u64;

        if let Err(err) = res {
            tracing::warn!(
                ?err,
                elapsed,
                data_kind = self.0.as_str(),
                "analytics export failed"
            );
        } else {
            tracing::info!(
                elapsed,
                data_kind = self.0.as_str(),
                "analytics export failed"
            );
        }
    }
}

#[derive(Clone)]
pub struct NotifyAnalytics {
    pub subscriber_notifications: ArcCollector<SubscriberNotification>,
    pub subscriber_updates: ArcCollector<SubscriberUpdate>,
    pub get_notifications: ArcCollector<GetNotifications>,
    pub mark_notifications_as_read: ArcCollector<MarkNotificationsAsRead>,
    pub notification_links: ArcCollector<NotificationLink>,
    pub relay_requests: ArcCollector<RelayRequest>,
    pub geoip_resolver: Option<Arc<MaxMindResolver>>,
}

impl NotifyAnalytics {
    pub fn with_noop_export() -> Self {
        info!("initializing analytics with noop export");

        Self {
            subscriber_notifications: analytics::noop_collector().boxed_shared(),
            subscriber_updates: analytics::noop_collector().boxed_shared(),
            get_notifications: analytics::noop_collector().boxed_shared(),
            mark_notifications_as_read: analytics::noop_collector().boxed_shared(),
            notification_links: analytics::noop_collector().boxed_shared(),
            relay_requests: analytics::noop_collector().boxed_shared(),
            geoip_resolver: None,
        }
    }

    pub fn with_aws_export(
        s3_client: S3Client,
        export_bucket: &str,
        node_addr: IpAddr,
        geoip_resolver: Option<Arc<MaxMindResolver>>,
    ) -> Self {
        fn make_export<T>(
            data_kind: DataKind,
            s3_client: S3Client,
            export_bucket: &str,
            node_addr: IpAddr,
        ) -> ArcCollector<T>
        where
            T: Sync + Send + 'static,
            [T]: RecordWriter<T>,
        {
            let observer = Observer(data_kind);
            BatchCollector::new(
                CollectorConfig {
                    data_queue_capacity: DATA_QUEUE_CAPACITY,
                    ..Default::default()
                },
                ParquetBatchFactory::new(Default::default()).with_observer(observer),
                AwsExporter::new(AwsConfig {
                    export_prefix: format!("notify/{}", data_kind.as_str()),
                    export_name: data_kind.as_str().to_string(),
                    node_addr,
                    file_extension: "parquet".to_owned(),
                    bucket_name: export_bucket.to_owned(),
                    s3_client,
                    upload_timeout: ANALYTICS_EXPORT_TIMEOUT,
                })
                .with_observer(observer),
            )
            .with_observer(observer)
            .boxed_shared()
        }

        Self {
            subscriber_notifications: make_export(
                DataKind::SubscriberNotifications,
                s3_client.clone(),
                export_bucket,
                node_addr,
            ),
            subscriber_updates: make_export(
                DataKind::SubscriberUpdates,
                s3_client.clone(),
                export_bucket,
                node_addr,
            ),
            get_notifications: make_export(
                DataKind::GetNotifications,
                s3_client.clone(),
                export_bucket,
                node_addr,
            ),
            mark_notifications_as_read: make_export(
                DataKind::MarkNotificationsAsRead,
                s3_client.clone(),
                export_bucket,
                node_addr,
            ),
            notification_links: make_export(
                DataKind::NotificationLinks,
                s3_client.clone(),
                export_bucket,
                node_addr,
            ),
            relay_requests: make_export(
                DataKind::RelayRequests,
                s3_client.clone(),
                export_bucket,
                node_addr,
            ),
            geoip_resolver,
        }
    }

    pub fn subscriber_notification(&self, event: SubscriberNotificationParams) {
        if let Err(err) = self.subscriber_notifications.collect(event.into()) {
            tracing::warn!(
                ?err,
                data_kind = DataKind::SubscriberNotifications.as_str(),
                "failed to collect analytics"
            );
        }
    }

    pub fn subscriber_update(&self, event: SubscriberUpdateParams) {
        if let Err(err) = self.subscriber_updates.collect(event.into()) {
            tracing::warn!(
                ?err,
                data_kind = DataKind::SubscriberUpdates.as_str(),
                "failed to collect analytics"
            );
        }
    }

    pub fn get_notifications(&self, event: GetNotificationsParams) {
        if let Err(err) = self.get_notifications.collect(event.into()) {
            tracing::warn!(
                ?err,
                data_kind = DataKind::GetNotifications.as_str(),
                "failed to collect analytics"
            );
        }
    }

    pub fn mark_notifications_as_read(&self, event: MarkNotificationsAsReadParams) {
        if let Err(err) = self.mark_notifications_as_read.collect(event.into()) {
            tracing::warn!(
                ?err,
                data_kind = DataKind::MarkNotificationsAsRead.as_str(),
                "failed to collect analytics"
            );
        }
    }

    pub fn notification_links(&self, event: NotificationLinkParams) {
        if let Err(err) = self.notification_links.collect(event.into()) {
            tracing::warn!(
                ?err,
                data_kind = DataKind::NotificationLinks.as_str(),
                "failed to collect analytics"
            );
        }
    }

    pub fn relay_request(&self, event: RelayResponseParams) {
        if let Err(err) = self.relay_requests.collect(event.into()) {
            tracing::warn!(
                ?err,
                data_kind = DataKind::RelayRequests.as_str(),
                "failed to collect analytics"
            );
        }
    }

    pub fn lookup_geo_data(&self, addr: IpAddr) -> Option<geoip::Data> {
        self.geoip_resolver
            .as_ref()?
            .lookup_geo_data(addr)
            .map_err(|err| {
                error!(?err, "failed to lookup geoip data");
                err
            })
            .ok()
    }
}

pub async fn initialize(
    config: &Configuration,
    s3_client: S3Client,
    geoip_resolver: Option<Arc<MaxMindResolver>>,
) -> NotifyAnalytics {
    if let Some(export_bucket) = config.analytics_export_bucket.as_deref() {
        NotifyAnalytics::with_aws_export(s3_client, export_bucket, config.public_ip, geoip_resolver)
    } else {
        NotifyAnalytics::with_noop_export()
    }
}
