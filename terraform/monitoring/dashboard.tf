data "jsonnet_file" "dashboard" {
  source = "${path.module}/dashboard.jsonnet"

  ext_str = {
    dashboard_title = "${var.environment} - Cast Server"
    dashboard_uid   = "${var.environment}-cast-server"

    prometheus_uid = grafana_data_source.prometheus.uid
    cloudwatch_uid = grafana_data_source.cloudwatch.uid

    environment      = var.environment
    notifications    = jsonencode(local.notifications)
    ecs_service_name = "${var.environment}-cast-keystore-docdb-primary-cluster"
    load_balancer    = local.load_balancer
    docdb_cluster_id = var.document_db_cluster_id
  }
}

resource "grafana_dashboard" "dashboard" {
  overwrite   = true
  message     = "Updated by Terraform"
  config_json = data.jsonnet_file.dashboard.rendered
}
