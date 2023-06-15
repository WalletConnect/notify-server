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
  default = "cast.walletconnect.com"
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

variable "keypair_seed" {
  type = string
}


variable "app_name" {
  type = string
}

variable "environment" {
  type = string
}

variable "geoip_db_key" {
  description = "The key to the GeoIP database"
  type        = string
  default     = "GeoLite2-City.mmdb"
}

variable "project_id" {
  description = "The project ID to use for billing purposes"
  type        = string
}

variable "relay_url" {
  description = "The URL of the relay server"
  type        = string
}

variable "cast_url" {
  description = "The URL of the cast server"
  type        = string
}