locals {
  image = "${var.ecr_repository_url}:${var.image_version}"

  otel_port   = var.port + 1
  otel_cpu    = 128
  otel_memory = 128

  file_descriptor_soft_limit = pow(2, 18)
  file_descriptor_hard_limit = local.file_descriptor_soft_limit * 2
}

module "ecs_cpu_mem" {
  source  = "app.terraform.io/wallet-connect/ecs_cpu_mem/aws"
  version = "1.0.0"
  cpu     = var.task_cpu + local.otel_cpu
  memory  = var.task_memory + local.otel_memory
}

#-------------------------------------------------------------------------------
# ECS Cluster

resource "aws_ecs_cluster" "app_cluster" {
  name = "${module.this.id}-cluster"

  configuration {
    execute_command_configuration {
      logging = "OVERRIDE"

      log_configuration {
        cloud_watch_encryption_enabled = false
        cloud_watch_log_group_name     = aws_cloudwatch_log_group.cluster.name
      }
    }
  }

  # Exposes metrics such as the number of running tasks in CloudWatch
  setting {
    name  = "containerInsights"
    value = "enabled"
  }
}

#-------------------------------------------------------------------------------
# ECS Task definition

resource "aws_ecs_task_definition" "app_task" {
  family                   = module.this.id
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc" # Using awsvpc as our network mode as this is required for Fargate
  cpu                      = module.ecs_cpu_mem.cpu
  memory                   = module.ecs_cpu_mem.memory
  execution_role_arn       = data.aws_iam_role.ecs_task_execution_role.arn
  task_role_arn            = data.aws_iam_role.ecs_task_execution_role.arn

  runtime_platform {
    operating_system_family = "LINUX"
  }

  container_definitions = jsonencode([
    {
      name      = module.this.id,
      image     = local.image,
      cpu       = var.task_cpu,
      memory    = var.task_memory,
      essential = true,

      environment = [for each in [
        { name = "PORT", value = tostring(var.port) },
        { name = "LOG_LEVEL", value = var.log_level },
        { name = "KEYPAIR_SEED", value = var.keypair_seed },
        { name = "PROJECT_ID", value = var.project_id },
        { name = "RELAY_URL", value = var.relay_url },
        { name = "NOTIFY_URL", value = var.notify_url },

        { name = "DATABASE_URL", value = var.docdb_url },
        { name = "POSTGRES_URL", value = var.postgres_url },

        { name = "REGISTRY_URL", value = var.registry_api_endpoint },
        { name = "REGISTRY_AUTH_TOKEN", value = var.registry_api_auth_token },

        { name = "REDIS_POOL_SIZE", value = tostring(var.redis_pool_size) },
        var.cache_endpoint_read != "" ? { name = "AUTH_REDIS_ADDR_READ", value = var.cache_endpoint_read } : null,
        var.cache_endpoint_write != "" ? { name = "AUTH_REDIS_ADDR_WRITE", value = var.cache_endpoint_write } : null,

        { name = "GEOIP_DB_BUCKET", value = var.geoip_db_bucket_name },
        { name = "GEOIP_DB_KEY", value = var.geoip_db_key },

        { name = "BLOCKED_COUNTRIES", value = join(",", var.ofac_blocked_countries) },

        { name = "ANALYTICS_ENABLED", value = "true" },
        { name = "ANALYTICS_EXPORT_BUCKET", value = var.analytics_datalake_bucket_name },

        { name = "TELEMETRY_ENABLED", value = "true" },
        { name = "TELEMETRY_PROMETHEUS_PORT", value = tostring(local.otel_port) },
        { name = "OTEL_TRACES_SAMPLER_ARG", value = tostring(var.telemetry_sample_ratio) },
      ] : each if each != null]

      portMappings = [
        {
          containerPort = var.port,
          hostPort      = var.port
        },
        {
          containerPort = local.otel_port,
          hostPort      = local.otel_port
        },
      ],

      ulimits : [
        {
          name      = "nofile",
          softLimit = local.file_descriptor_soft_limit,
          hardLimit = local.file_descriptor_hard_limit,
        }
      ],

      logConfiguration : {
        logDriver = "awslogs",
        options = {
          "awslogs-group"         = aws_cloudwatch_log_group.cluster.name,
          "awslogs-region"        = module.this.region,
          "awslogs-stream-prefix" = "ecs"
        }
      },

      dependsOn = [
        { containerName : "aws-otel-collector", condition : "START" },
      ]
    },

    # Forward telemetry data to AWS CloudWatch
    {
      name      = "aws-otel-collector",
      image     = "public.ecr.aws/aws-observability/aws-otel-collector:latest",
      cpu       = local.otel_cpu,
      memory    = local.otel_memory,
      essential = true,

      command = [
        "--config=/etc/ecs/ecs-amp-prometheus.yaml",
        # Uncomment to enable debug logging in otel-collector
        # "--set=service.telemetry.logs.level=DEBUG"
      ],

      environment = [
        { name : "AWS_PROMETHEUS_SCRAPING_ENDPOINT", value : "0.0.0.0:${local.otel_port}" },
        { name : "AWS_PROMETHEUS_ENDPOINT", value : "${var.prometheus_endpoint}api/v1/remote_write" },
        { name : "AWS_REGION", value : module.this.region },
      ],

      logConfiguration = {
        logDriver = "awslogs",
        options = {
          "awslogs-group"         = aws_cloudwatch_log_group.otel.name,
          "awslogs-region"        = module.this.region,
          "awslogs-stream-prefix" = "ecs"
        }
      }
    },
  ])
}


#-------------------------------------------------------------------------------
# ECS Service

resource "aws_ecs_service" "app_service" {
  name            = "${module.this.id}-service"
  cluster         = aws_ecs_cluster.app_cluster.id
  task_definition = aws_ecs_task_definition.app_task.arn
  launch_type     = "FARGATE"
  desired_count   = var.autoscaling_desired_count
  propagate_tags  = "TASK_DEFINITION"

  # Wait for the service deployment to succeed
  wait_for_steady_state = true

  network_configuration {
    subnets          = var.private_subnets
    assign_public_ip = false
    security_groups  = [aws_security_group.app_ingress.id]
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.target_group.arn
    container_name   = aws_ecs_task_definition.app_task.family
    container_port   = var.port
  }

  # Allow external changes without Terraform plan difference
  lifecycle {
    ignore_changes = [desired_count]
  }
}
