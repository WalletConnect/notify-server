use {
    self::{
        notify_client::NotifyClientParams,
        notify_message::{NotifyMessage, NotifyMessageParams},
    },
    crate::{analytics::notify_client::NotifyClient, config::Configuration, error::Result},
    aws_sdk_s3::Client as S3Client,
    std::{net::IpAddr, sync::Arc},
    tracing::{error, info},
    wc::{
        analytics::{
            collectors::{batch::BatchOpts, noop::NoopCollector},
            exporters::aws::{AwsExporter, AwsOpts},
            writers::parquet::ParquetWriter,
            Analytics,
        },
        geoip::{self, MaxMindResolver, Resolver},
    },
};

pub mod notify_client;
pub mod notify_message;

#[derive(Clone)]
pub struct NotifyAnalytics {
    pub messages: Analytics<NotifyMessage>,
    pub clients: Analytics<NotifyClient>,
    pub geoip_resolver: Option<Arc<MaxMindResolver>>,
}

impl NotifyAnalytics {
    pub fn with_noop_export() -> Self {
        info!("initializing analytics with noop export");

        Self {
            messages: Analytics::new(NoopCollector),
            clients: Analytics::new(NoopCollector),
            geoip_resolver: None,
        }
    }

    pub fn with_aws_export(
        s3_client: S3Client,
        export_bucket: &str,
        node_ip: IpAddr,
        geoip_resolver: Option<Arc<MaxMindResolver>>,
    ) -> anyhow::Result<Self> {
        info!(%export_bucket, "initializing analytics with aws export");

        let opts = BatchOpts::default();
        let bucket_name: Arc<str> = export_bucket.into();
        let node_ip: Arc<str> = node_ip.to_string().into();

        let messages = {
            let exporter = AwsExporter::new(AwsOpts {
                export_prefix: "notify/messages",
                export_name: "messages",
                file_extension: "parquet",
                bucket_name: bucket_name.clone(),
                s3_client: s3_client.clone(),
                node_ip: node_ip.clone(),
            });

            let collector = ParquetWriter::<NotifyMessage>::new(opts.clone(), exporter)?;
            Analytics::new(collector)
        };

        let clients = {
            let exporter = AwsExporter::new(AwsOpts {
                export_prefix: "notify/clients",
                export_name: "clients",
                file_extension: "parquet",
                bucket_name,
                s3_client,
                node_ip,
            });

            Analytics::new(ParquetWriter::<NotifyClient>::new(opts, exporter)?)
        };

        Ok(Self {
            messages,
            clients,
            geoip_resolver,
        })
    }

    pub fn message(&self, message: NotifyMessageParams) {
        self.messages.collect(message.into());
    }

    pub fn client(&self, client: NotifyClientParams) {
        self.clients.collect(client.into());
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
) -> Result<NotifyAnalytics> {
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
