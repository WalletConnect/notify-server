variable "region" {
  type    = string
  default = "eu-central-1"
}

#variable "azs" {
#  type    = list(string)
#  default = ["eu-central-1a", "eu-central-1b", "eu-central-1c"]
#}

variable "public_url" {
  type    = string
  default = "notify.walletconnect.com"
}

variable "grafana_endpoint" {
  type = string
}

variable "image_version" {
  type    = string
  default = ""
}

variable "docdb_primary_instance_class" {
  type = string
}

variable "docdb_primary_instances" {
  type = number
}

variable "docdb_replica_instance_class" {
  type = string
}

variable "docdb_replica_instances" {
  type = number
}

variable "keypair_seed" {
  type = string
}


variable "app_name" {
  type = string
}

variable "environment" {
  type = string
}

variable "project_id" {
  description = "The project ID to use for billing purposes"
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

variable "data_lake_kms_key_arn" {
  description = "The ARN of KMS encryption key for the data-lake bucket."
  type        = string
}

variable "registry_url" {
  description = "The url of registry with project data"
  type        = string
}

variable "registry_auth_token" {
  description = "The auth token for registry"
  type        = string
}

variable "grafana_auth" {
  description = "Grafana auth token"
  type        = string
}
