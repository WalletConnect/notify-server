# If you change instance memory capacity, you must adjust `docdb_mem` in panels.libsonnet
docdb_primary_instance_class = "db.r5.large"
docdb_primary_instances      = 1
docdb_replica_instance_class = "db.r5.large"
docdb_replica_instances      = 1
grafana_endpoint             = "g-e66b9a1a24.grafana-workspace.eu-central-1.amazonaws.com"


notify_url  = "https://notify.walletconnect.com"
app_name    = "notify"
environment = "prod"

data_lake_kms_key_arn = "arn:aws:kms:eu-central-1:898587786287:key/06e7c9fd-943d-47bf-bcf4-781b44411ba4"
relay_url             = "wss://relay.walletconnect.com"
registry_url          = "https://registry.walletconnect.com"
