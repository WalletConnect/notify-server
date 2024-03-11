use {
    self::{
        get_notifications::{GetNotifications, GetNotificationsParams},
        subscriber_notification::{SubscriberNotification, SubscriberNotificationParams},
        subscriber_update::{SubscriberUpdate, SubscriberUpdateParams},
    },
    crate::config::Configuration,
    aws_sdk_s3::Client as S3Client,
    std::{net::IpAddr, sync::Arc},
    tracing::{error, info},
    wc::{
        analytics::{
            collectors::{
                batch::{BatchError, BatchOpts},
                noop::NoopCollector,
                BatchWriter,
            },
            exporters::aws::{AwsExporter, AwsOpts},
            writers::parquet::ParquetWriter,
            Analytics,
        },
        geoip::{self, MaxMindResolver, Resolver},
    },
};

pub mod get_notifications;
pub mod subscriber_notification;
pub mod subscriber_update;

#[derive(Clone)]
pub struct NotifyAnalytics {
    pub subscriber_notifications: Analytics<SubscriberNotification>,
    pub subscriber_updates: Analytics<SubscriberUpdate>,
    pub get_notifications: Analytics<GetNotifications>,
    pub geoip_resolver: Option<Arc<MaxMindResolver>>,
}

impl NotifyAnalytics {
    pub fn with_noop_export() -> Self {
        info!("initializing analytics with noop export");

        Self {
            subscriber_notifications: Analytics::new(NoopCollector),
            subscriber_updates: Analytics::new(NoopCollector),
            get_notifications: Analytics::new(NoopCollector),
            geoip_resolver: None,
        }
    }

    pub fn with_aws_export(
        s3_client: S3Client,
        export_bucket: &str,
        node_ip: IpAddr,
        geoip_resolver: Option<Arc<MaxMindResolver>>,
    ) -> Result<Self, AnalyticsInitError> {
        info!(%export_bucket, "initializing analytics with aws export");

        let opts = BatchOpts::default();
        let bucket_name: Arc<str> = export_bucket.into();
        let node_ip: Arc<str> = node_ip.to_string().into();

        let subscriber_notifications = {
            let exporter = AwsExporter::new(AwsOpts {
                export_prefix: "notify/subscriber_notifications",
                export_name: "subscriber_notifications",
                file_extension: "parquet",
                bucket_name: bucket_name.clone(),
                s3_client: s3_client.clone(),
                node_ip: node_ip.clone(),
            });

            Analytics::new(ParquetWriter::new(opts.clone(), exporter)?)
        };

        let subscriber_updates = {
            let exporter = AwsExporter::new(AwsOpts {
                export_prefix: "notify/subscriber_updates",
                export_name: "subscriber_updates",
                file_extension: "parquet",
                bucket_name: bucket_name.clone(),
                s3_client: s3_client.clone(),
                node_ip: node_ip.clone(),
            });

            Analytics::new(ParquetWriter::new(opts.clone(), exporter)?)
        };

        let get_notifications = {
            let exporter = AwsExporter::new(AwsOpts {
                export_prefix: "notify/get_notifications",
                export_name: "get_notifications",
                file_extension: "parquet",
                bucket_name: bucket_name.clone(),
                s3_client: s3_client.clone(),
                node_ip: node_ip.clone(),
            });

            Analytics::new(ParquetWriter::new(opts.clone(), exporter)?)
        };

        Ok(Self {
            subscriber_notifications,
            subscriber_updates,
            get_notifications,
            geoip_resolver,
        })
    }

    pub fn subscriber_notification(&self, event: SubscriberNotificationParams) {
        self.subscriber_notifications.collect(event.into());
    }

    pub fn subscriber_update(&self, event: SubscriberUpdateParams) {
        self.subscriber_updates.collect(event.into());
    }

    pub fn get_notifications(&self, event: GetNotificationsParams) {
        self.get_notifications.collect(event.into());
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
) -> Result<NotifyAnalytics, AnalyticsInitError> {
    if let Some(export_bucket) = config.analytics_export_bucket.as_deref() {
        Ok(NotifyAnalytics::with_aws_export(
            s3_client,
            export_bucket,
            config.public_ip,
            geoip_resolver,
        )?)
    } else {
        Ok(NotifyAnalytics::with_noop_export())
    }
}

type ParquetError<T> = BatchError<<ParquetWriter<T> as BatchWriter<T>>::Error>;

#[derive(thiserror::Error, Debug)]
pub enum AnalyticsInitError {
    #[error("SubscriberNotification error")]
    SubscriberNotificationError(#[from] ParquetError<SubscriberNotification>),
}
