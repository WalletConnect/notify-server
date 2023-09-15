locals {
  service_name = "${var.app_name}-service"

  file_descriptor_soft_limit = pow(2, 18)
  file_descriptor_hard_limit = local.file_descriptor_soft_limit * 2

  prometheus_port = "8081"

}

# Log Group for our App
resource "aws_cloudwatch_log_group" "cluster_logs" {
  name              = "${var.app_name}_logs"
  retention_in_days = 14
}

# ECS Cluster
resource "aws_ecs_cluster" "app_cluster" {
  name = var.app_name

  configuration {
    execute_command_configuration {
      logging = "OVERRIDE"

      log_configuration {
        cloud_watch_encryption_enabled = false
        cloud_watch_log_group_name     = aws_cloudwatch_log_group.cluster_logs.name
      }
    }
  }
}

## Task Definition
resource "aws_ecs_task_definition" "app_task_definition" {
  family = var.app_name
  cpu    = var.cpu
  memory = var.memory
  requires_compatibilities = [
    "FARGATE"
  ]
  network_mode       = "awsvpc" # Required because of fargate
  execution_role_arn = aws_iam_role.ecs_task_execution_role.arn
  task_role_arn      = aws_iam_role.ecs_task_execution_role.arn
  container_definitions = jsonencode([
    {
      name  = var.app_name,
      image = var.image,
      cpu   = var.cpu - 128, # Remove sidecar memory/cpu so rest is assigned to primary container
      ulimits = [{
        name : "nofile",
        softLimit : local.file_descriptor_soft_limit,
        hardLimit : local.file_descriptor_hard_limit
      }],
      memory    = var.memory - 128,
      essential = true,
      portMappings = [
        {
          containerPort = 8080,
          hostPort      = 8080
        },
        {
          containerPort = 8081,
          hostPort      = 8081
        }
      ],
      environment = [
        { name = "PORT", value = "8080" },
        { name = "LOG_LEVEL", value = "info,hyper::proto::h1=trace" },
        { name = "TELEMETRY_ENABLED", value = "true" },
        { name = "TELEMETRY_PROMETHEUS_PORT", value = local.prometheus_port },
        { name = "OTEL_EXPORTER_OTLP_ENDPOINT", value = "http://localhost:4317" },
        { name = "TELEMETRY_PROMETHEUS_PORT", value = "8081" },
        { name = "DATABASE_URL", value = var.mongo_address },
        { name = "KEYPAIR_SEED", value = var.keypair_seed },
        { name = "OTEL_TRACES_SAMPLER_ARG", value = tostring(var.telemetry_sample_ratio) },
        { name = "ANALYTICS_ENABLED", value = "true" },
        { name = "ANALYTICS_EXPORT_BUCKET", value = var.data_lake_bucket_name },
        { name = "ANALYTICS_GEOIP_DB_BUCKET", value = var.analytics_geoip_db_bucket_name },
        { name = "ANALYTICS_GEOIP_DB_KEY", value = var.geoip_db_key },
        { name = "PROJECT_ID", value = var.project_id },
        { name = "RELAY_URL", value = var.relay_url },
        { name = "NOTIFY_URL", value = var.notify_url },
        { name = "REGISTRY_URL", value = var.registry_url },
        { name = "REGISTRY_AUTH_TOKEN", value = var.registry_auth_token },
        { name = "REDIS_POOL_SIZE", value = var.redis_pool_size },
      ],
      dependsOn = [
        { containerName = "aws-otel-collector", condition = "START" }
      ],
      logConfiguration = {
        logDriver = "awslogs",
        options = {
          awslogs-group         = aws_cloudwatch_log_group.cluster_logs.name,
          awslogs-region        = var.region,
          awslogs-stream-prefix = "ecs"
        }
      }
    },
    {
      name   = "aws-otel-collector",
      image  = "public.ecr.aws/aws-observability/aws-otel-collector:latest",
      cpu    = 128,
      memory = 128,
      environment = [
        { "name" : "AWS_PROMETHEUS_SCRAPING_ENDPOINT", "value" : "0.0.0.0:8081" },
        { name = "AWS_PROMETHEUS_ENDPOINT", value = "${var.prometheus_endpoint}api/v1/remote_write" },
        { name = "AWS_REGION", value = "eu-central-1" }
      ],
      essential = true,
      command = [
        "--config=/etc/ecs/ecs-amp-xray-prometheus.yaml"
      ],
      logConfiguration = {
        logDriver = "awslogs",
        options = {
          awslogs-create-group  = "True",
          awslogs-group         = "/ecs/${var.app_name}-ecs-aws-otel-sidecar-collector",
          awslogs-region        = var.region,
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])

  runtime_platform {
    operating_system_family = "LINUX"
  }
}

## Service
resource "aws_ecs_service" "app_service" {
  name            = local.service_name
  cluster         = aws_ecs_cluster.app_cluster.id
  task_definition = aws_ecs_task_definition.app_task_definition.arn
  launch_type     = "FARGATE"
  desired_count   = 1
  propagate_tags  = "TASK_DEFINITION"

  # Wait for the service deployment to succeed
  wait_for_steady_state = true

  # Allow external changes without Terraform plan difference
  lifecycle {
    ignore_changes = [desired_count]
  }

  network_configuration {
    subnets          = var.private_subnets
    assign_public_ip = true                                # We do public ingress through the LB
    security_groups  = [aws_security_group.app_ingress.id] # Setting the security group
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.target_group.arn # Referencing our target group
    container_name   = var.app_name
    container_port   = 8080 # Specifying the container port
  }
}

# Load Balancers & Networking
resource "aws_lb" "application_load_balancer" {
  name               = "${var.app_name}-load-balancer"
  load_balancer_type = "application"
  subnets            = var.public_subnets

  security_groups = [aws_security_group.lb_ingress.id]
}



resource "aws_lb_target_group" "target_group" {
  name        = "${var.app_name}-target-group"
  port        = 8080
  protocol    = "HTTP"
  target_type = "ip"
  vpc_id      = var.vpc_id # Referencing the default VPC
  slow_start  = 30         # Give a 30 second delay to allow the service to startup

  health_check {
    protocol            = "HTTP"
    path                = "/health" # Notify Server's health path
    port                = 8080
    interval            = 15
    timeout             = 10
    healthy_threshold   = 3
    unhealthy_threshold = 3
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_lb_listener" "listener" {
  load_balancer_arn = aws_lb.application_load_balancer.arn # Referencing our load balancer
  port              = "443"
  protocol          = "HTTPS"
  certificate_arn   = var.acm_certificate_arn

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.target_group.arn # Referencing our target group
  }
}

resource "aws_lb_listener" "listener-http" {
  load_balancer_arn = aws_lb.application_load_balancer.arn
  port              = "80"
  protocol          = "HTTP"

  default_action {
    type = "redirect"

    redirect {
      port        = "443"
      protocol    = "HTTPS"
      status_code = "HTTP_301"
    }
  }
}

# DNS Records
resource "aws_route53_record" "dns_load_balancer" {
  zone_id = var.route53_zone_id
  name    = var.fqdn
  type    = "A"

  alias {
    name                   = aws_lb.application_load_balancer.dns_name
    zone_id                = aws_lb.application_load_balancer.zone_id
    evaluate_target_health = true
  }
}


# Security Groups
resource "aws_security_group" "app_ingress" {
  name        = "${var.app_name}-ingress-to-app"
  description = "Allow app port ingress"
  vpc_id      = var.vpc_id

  ingress {
    from_port       = 0
    to_port         = 0
    protocol        = "-1"
    security_groups = [aws_security_group.lb_ingress.id]
  }

  ingress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = [var.vpc_cidr]
  }

  egress {
    from_port   = 0             # Allowing any incoming port
    to_port     = 0             # Allowing any outgoing port
    protocol    = "-1"          # Allowing any outgoing protocol
    cidr_blocks = ["0.0.0.0/0"] # Allowing traffic out to all IP addresses
  }

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_security_group" "lb_ingress" {
  name        = "${var.app_name}-lb-ingress"
  description = "Allow app port ingress from vpc"
  vpc_id      = var.vpc_id

  ingress {
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"] # Allowing traffic in from all sources
  }

  ingress {
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"] # Allowing traffic in from all sources
  }

  egress {
    from_port   = 0              # Allowing any incoming port
    to_port     = 0              # Allowing any outgoing port
    protocol    = "-1"           # Allowing any outgoing protocol
    cidr_blocks = [var.vpc_cidr] # Allowing traffic out to all VPC IP addresses
  }

  lifecycle {
    create_before_destroy = true
  }
}
