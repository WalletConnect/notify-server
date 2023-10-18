#-------------------------------------------------------------------------------
# Cluster

variable "ecr_repository_url" {
  description = "The URL of the ECR repository where the app image is stored"
  type        = string
}

variable "image_version" {
  description = "The version of the app image to deploy"
  type        = string
}

variable "task_execution_role_name" {
  description = "The name of the task execution role"
  type        = string
}

variable "task_cpu" {
  description = "The number of CPU units to reserve for the container."
  type        = number
}

variable "task_memory" {
  description = "The amount of memory (in MiB) to reserve for the container."
  type        = number
}

variable "autoscaling_desired_count" {
  description = "Minimum number of instances in the autoscaling group"
  type        = number
  default     = 1
}

variable "autoscaling_min_capacity" {
  description = "Minimum number of instances in the autoscaling group"
  type        = number
  default     = 1
}

variable "autoscaling_max_capacity" {
  description = "Maximum number of instances in the autoscaling group"
  type        = number
  default     = 1
}

#-------------------------------------------------------------------------------
# DNS

variable "route53_zones" {
  description = "The FQDNs to use for the app"
  type        = map(string)
}

variable "route53_zones_certificates" {
  description = "The ARNs of the ACM certificates to use for HTTPS"
  type        = map(string)
}

#-------------------------------------------------------------------------------
# Network

variable "vpc_id" {
  description = "The ID of the VPC to deploy to"
  type        = string
}

variable "public_subnets" {
  description = "The IDs of the public subnets"
  type        = list(string)
}

variable "private_subnets" {
  description = "The IDs of the private subnets"
  type        = list(string)
}

variable "database_subnets" {
  description = "The IDs of the database subnets"
  type        = list(string)
}

variable "allowed_app_ingress_cidr_blocks" {
  description = "A list of CIDR blocks to allow ingress access to the application."
  type        = string
}

variable "allowed_lb_ingress_cidr_blocks" {
  description = "A list of CIDR blocks to allow ingress access to the load-balancer."
  type        = string
}

#-------------------------------------------------------------------------------
# Application

variable "port" {
  description = "The port the app listens on"
  type        = number
}

variable "log_level" {
  description = "The log level for the app"
  type        = string
}

variable "keypair_seed" {
  description = "The seed for the keypair used to encrypt data"
  type        = string
}

variable "project_id" {
  description = "The ID of the project to use for the app"
  type        = string
}

variable "relay_url" {
  description = "The URL of the relay server"
  type        = string
}

variable "notify_url" {
  description = "The URL of the notify server"
  type        = string
}

variable "redis_pool_size" {
  description = "The size of the Redis connection pool"
  type        = number
  default     = 128
}

variable "cache_endpoint_read" {
  description = "The endpoint of the cache (read)"
  type        = string
  default     = null
}

variable "cache_endpoint_write" {
  description = "The endpoint of the cache (write)"
  type        = string
  default     = null
}

variable "docdb_url" {
  description = "The connection URL for the MongoDB instance"
  type        = string
}

variable "postgres_url" {
  description = "The connection URL for the PostgreSQL instance"
  type        = string
}

variable "rds_auth_policy_arn" {
  description = "The ARN for the rds_auth_policy aws_iam_role_policy"
  type        = string
}

#-------------------------------------------------------------------------------
# Project Registry

variable "registry_api_endpoint" {
  description = "The endpoint of the registry API"
  type        = string
}

variable "registry_api_auth_token" {
  description = "The auth token for the registry API"
  type        = string
}


#-------------------------------------------------------------------------------
# Analytics

variable "analytics_datalake_bucket_name" {
  description = "The name of the S3 bucket to use for the analytics datalake"
  type        = string
}

variable "analytics_datalake_kms_key_arn" {
  description = "The ARN of the KMS key to use with the datalake bucket"
  type        = string
}

#-------------------------------------------------------------------------------
# Autoscaling

variable "autoscaling_cpu_target" {
  description = "The target CPU utilization for the autoscaling group"
  type        = number
  default     = 50
}

variable "autoscaling_cpu_scale_in_cooldown" {
  description = "The cooldown period (in seconds) before a scale in is possible"
  type        = number
  default     = 180
}

variable "autoscaling_cpu_scale_out_cooldown" {
  description = "The cooldown period (in seconds) before a scale out is possible"
  type        = number
  default     = 180
}

variable "autoscaling_memory_target" {
  description = "The target memory utilization for the autoscaling group"
  type        = number
  default     = 50
}

variable "autoscaling_memory_scale_in_cooldown" {
  description = "The cooldown period (in seconds) before a scale in is possible"
  type        = number
  default     = 180
}

variable "autoscaling_memory_scale_out_cooldown" {
  description = "The cooldown period (in seconds) before a scale out is possible"
  type        = number
  default     = 180
}

#-------------------------------------------------------------------------------
# Monitoring

variable "prometheus_endpoint" {
  description = "The endpoint of the Prometheus server to use for monitoring"
  type        = string
}

variable "telemetry_sample_ratio" {
  description = "The sample ratio for telemetry data"
  type        = number
}

#---------------------------------------
# GeoIP

variable "geoip_db_bucket_name" {
  description = "The name of the S3 bucket where the GeoIP database is stored"
  type        = string
}

variable "geoip_db_key" {
  description = "The key of the GeoIP database in the S3 bucket"
  type        = string
}

#---------------------------------------
# OFAC

variable "ofac_blocked_countries" {
  description = "The list of countries under OFAC sanctions"
  type        = list(string)
  default     = ["KP", "IR", "CU", "SY"]
}
