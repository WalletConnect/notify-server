locals {
  telemetry_sample_ratio = local.stage == "prod" ? 0.25 : 1
}

resource "aws_iam_role" "application_role" {
  name = "${module.this.id}-ecs-task-execution"
  assume_role_policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ecs-tasks.amazonaws.com"
        }
      }
    ]
  })
}

data "aws_s3_bucket" "geoip" {
  bucket = data.terraform_remote_state.infra_aws.outputs.geoip_bucked_id
}

resource "aws_prometheus_workspace" "prometheus" {
  alias = "prometheus-${module.this.id}"
}

# ECS Cluster, Task, Service, and Load Balancer for our app
module "ecs" {
  source  = "./ecs"
  context = module.this

  # Cluster
  ecr_repository_url        = local.ecr_repository_url
  image_version             = var.image_version
  task_execution_role_name  = aws_iam_role.application_role.name
  task_cpu                  = 1024
  task_memory               = 2048
  autoscaling_desired_count = var.app_autoscaling_desired_count
  autoscaling_min_capacity  = var.app_autoscaling_min_capacity
  autoscaling_max_capacity  = var.app_autoscaling_max_capacity
  cloudwatch_logs_key_arn   = aws_kms_key.cloudwatch_logs.arn

  # DNS
  route53_zones              = local.zones
  route53_zones_certificates = local.zones_certificates

  # Network
  vpc_id                          = module.vpc.vpc_id
  public_subnets                  = module.vpc.public_subnets
  private_subnets                 = module.vpc.private_subnets
  allowed_app_ingress_cidr_blocks = module.vpc.vpc_cidr_block
  allowed_lb_ingress_cidr_blocks  = module.vpc.vpc_cidr_block

  # Application
  port                   = 8080
  log_level              = var.log_level
  keypair_seed           = var.keypair_seed
  project_id             = var.project_id
  relay_url              = var.relay_url
  relay_public_key       = var.relay_public_key
  notify_url             = var.notify_url
  ofac_blocked_countries = var.ofac_blocked_countries

  postgres_url             = module.postgres.database_url
  postgres_max_connections = var.postgres_max_connections

  redis_pool_size      = 64
  cache_endpoint_read  = module.redis.endpoint
  cache_endpoint_write = module.redis.endpoint

  # Project Registry
  registry_api_endpoint   = var.registry_api_endpoint
  registry_api_auth_token = var.registry_api_auth_token

  # Analytics
  analytics_datalake_bucket_name = data.terraform_remote_state.datalake.outputs.datalake_bucket_id
  analytics_datalake_kms_key_arn = data.terraform_remote_state.datalake.outputs.datalake_kms_key_arn

  # Monitoring
  telemetry_sample_ratio = local.telemetry_sample_ratio
  prometheus_endpoint    = aws_prometheus_workspace.prometheus.prometheus_endpoint

  # GeoIP
  geoip_db_bucket_name = data.aws_s3_bucket.geoip.id
  geoip_db_key         = var.geoip_db_key

  depends_on = [aws_iam_role.application_role]
}
