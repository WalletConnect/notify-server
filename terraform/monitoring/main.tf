data "aws_caller_identity" "this" {}

data "jsonnet_file" "dashboard" {
  source = "${path.module}/dashboard.jsonnet"

  ext_str = {
    dashboard_title = "Notify Server - ${title(module.this.stage)}"
    dashboard_uid   = "notify-${module.this.stage}"

    prometheus_uid = grafana_data_source.prometheus.uid
    cloudwatch_uid = grafana_data_source.cloudwatch.uid

    environment   = module.this.stage
    notifications = jsonencode(var.notification_channels)

    ecs_cluster_name   = var.ecs_cluster_name
    ecs_service_name   = var.ecs_service_name
    rds_cluster_id     = var.rds_cluster_id
    redis_cluster_id   = var.redis_cluster_id
    load_balancer      = var.load_balancer_arn
    target_group       = var.ecs_target_group_arn
    log_group_app_name = var.log_group_app_name
    log_group_app_arn  = var.log_group_app_arn
    aws_account_id     = data.aws_caller_identity.current.account_id
  }
}

resource "grafana_dashboard" "main" {
  overwrite   = true
  message     = "Updated by Terraform"
  config_json = data.jsonnet_file.dashboard.rendered
}
