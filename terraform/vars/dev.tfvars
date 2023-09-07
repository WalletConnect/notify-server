docdb_primary_instance_class = "db.r5.large"
docdb_primary_instances      = 1
docdb_replica_instance_class = "db.r5.large"
docdb_replica_instances      = 0
grafana_endpoint             = "g-e66b9a1a24.grafana-workspace.eu-central-1.amazonaws.com"


notify_url  = "https://dev.notify.walletconnect.com/"
app_name    = "notify"
environment = "dev"

data_lake_kms_key_arn = "arn:aws:kms:eu-central-1:898587786287:key/d1d2f047-b2a3-4f4a-8786-7c87ee83c954"
relay_url             = "wss://staging.relay.walletconnect.com"

# Different from .env.example so they don't conflict
keypair_seed = "this-is-totally-secure"
