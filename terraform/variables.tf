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

variable "notify_url" {
  description = "The URL of the notify server"
  type        = string
}


#-------------------------------------------------------------------------------
# Database

variable "docdb_primary_instance_count" {
  description = "The number of primary docdb instances to deploy"
  type        = number
}

variable "docdb_primary_instance_class" {
  description = "The instance class of the primary docdb instances"
  type        = string
}

variable "docdb_replica_instance_count" {
  description = "The number of replica docdb instances to deploy"
  type        = number
}

variable "docdb_replica_instance_class" {
  description = "The instance class of the replica docdb instances"
  type        = string
}

variable "docdb_master_password" {
  description = "The master password for the docdb cluster"
  type        = string
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
}

variable "webhook_prometheus_p2" {
  description = "The webhook to send Prometheus P2 alerts to"
  type        = string
}
