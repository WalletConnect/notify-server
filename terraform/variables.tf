#-------------------------------------------------------------------------------
# Configuration

variable "grafana_auth" {
  description = "The API Token for the Grafana instance"
  type        = string
  default     = ""
}

#-------------------------------------------------------------------------------
# Application

variable "name" {
  description = "The name of the application"
  type        = string
  default     = "notify-server"
}

variable "region" {
  description = "AWS region to deploy to"
  type        = string
}

variable "image_version" {
  description = "The version of the image to deploy"
  type        = string
}

variable "log_level" {
  description = "Defines logging level for the application"
  type        = string
}

variable "app_autoscaling_desired_count" {
  description = "The desired number of tasks to run"
  type        = number
  default     = 2
}

variable "app_autoscaling_min_capacity" {
  description = "The minimum number of tasks to run when autoscaling"
  type        = number
  default     = 2
}

variable "app_autoscaling_max_capacity" {
  description = "The maximum number of tasks to run when autoscaling"
  type        = number
  default     = 8
}

variable "ofac_blocked_countries" {
  description = "The list of countries to block"
  type        = string
  default     = ""
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

variable "relay_public_key" {
  description = "The public key of the relay server obtained from relay.walletconnect.com/public-key"
  type        = string
}

variable "notify_url" {
  description = "The URL of the notify server"
  type        = string
}

variable "postgres_max_connections" {
  description = "The max connections in the PostgreSQL connection pool"
  type        = number
  default     = 100
}


#-------------------------------------------------------------------------------
# Analytics

variable "geoip_db_key" {
  description = "The name to the GeoIP database"
  type        = string
}

#-------------------------------------------------------------------------------
# Alerting / Monitoring

variable "notification_channels" {
  description = "The notification channels to send alerts to"
  type        = list(any)
  default     = []
}

variable "webhook_cloudwatch_p2" {
  description = "The webhook to send CloudWatch P2 alerts to"
  type        = string
  default     = ""
}

variable "webhook_prometheus_p2" {
  description = "The webhook to send Prometheus P2 alerts to"
  type        = string
  default     = ""
}
