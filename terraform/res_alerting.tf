module "alerting" {
  source  = "./alerting"
  context = module.this.context

  webhook_cloudwatch_p2 = var.webhook_cloudwatch_p2
  webhook_prometheus_p2 = var.webhook_prometheus_p2

  ecs_cluster_name = module.ecs.ecs_cluster_name
  ecs_service_name = module.ecs.ecs_service_name

  elb_load_balancer_arn = module.ecs.load_balancer_arn_suffix

  docdb_cluster_id = module.docdb.cluster_id
  redis_cluster_id = module.redis.cluster_id
}
